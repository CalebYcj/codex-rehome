import { render, screen, within } from "@testing-library/react";
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
  pickDirectory: vi.fn(),
  pickRehomePackage: vi.fn(),
  pickRehomeSavePath: vi.fn(),
  rollbackTransaction: vi.fn(),
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
    },
  ],
  conversation_paths: ["C:\\Users\\Me\\.codex\\sessions\\desktop.jsonl"],
  session_index_path: "C:\\Users\\Me\\.codex\\session_index.jsonl",
  state_db_path: "C:\\Users\\Me\\.codex\\state_5.sqlite",
  skill_paths: [],
  plugin_paths: [],
  generated_image_paths: [],
  warnings: [],
};

const preview = {
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
  changed_files: 8,
};

beforeEach(() => {
  vi.clearAllMocks();
  api.discoverCodex.mockResolvedValue(inventory);
  api.listTransactions.mockResolvedValue([]);
  api.pickRehomePackage.mockResolvedValue(preview.package_path);
  api.pickRehomeSavePath.mockResolvedValue("C:\\Transfers\\handoff.rehome");
  api.pickDirectory.mockImplementation(async (title: string) =>
    title.includes("备份") ? "C:\\ReHome Backups" : "C:\\Restored Projects",
  );
  api.inspectPackage.mockResolvedValue(preview);
  api.buildRestorePlan.mockResolvedValue(basePlan);
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
  await user.click(screen.getByRole("button", { name: "选择项目目录" }));
  await user.click(screen.getByRole("button", { name: "选择备份目录" }));
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

  it("requires a project and a conversation or content category before package creation", async () => {
    const user = userEvent.setup();
    render(<App />);
    await screen.findByText(inventory.codex_home);

    await user.click(screen.getByRole("button", { name: "前往发送" }));
    const createButton = screen.getByRole("button", { name: "创建 ReHome 包" });
    expect(createButton).toBeDisabled();

    await user.click(screen.getByRole("checkbox", { name: "选择项目 rehome-app" }));
    expect(createButton).toBeDisabled();

    await user.click(screen.getByRole("checkbox", { name: "选择对话 Desktop workflow" }));
    expect(createButton).toBeEnabled();
  });

  it("offers project and conversation paths from the current discovery contract", async () => {
    const user = userEvent.setup();
    api.discoverCodex.mockResolvedValue({
      ...inventory,
      projects: [],
      conversations: [],
      conversation_paths: [
        "C:\\Users\\Me\\.codex\\sessions\\44444444-4444-4444-4444-444444444444.jsonl",
      ],
    });
    render(<App />);
    await screen.findByText(inventory.codex_home);

    await user.click(screen.getByRole("button", { name: "前往发送" }));

    expect(screen.getByRole("checkbox", { name: "选择项目 rehome-app" })).toBeInTheDocument();
    expect(
      screen.getByRole("checkbox", { name: "选择对话 对话 44444444" }),
    ).toBeInTheDocument();
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
    expect(screen.getAllByText("C:\\Restored Projects").length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "开始恢复" })).toBeDisabled();
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

    await user.click(screen.getByRole("checkbox", { name: "确认 Codex 已关闭" }));
    await user.click(screen.getByRole("button", { name: "开始恢复" }));

    expect(
      await screen.findByText("项目文件已恢复，需要在 Codex 中手动打开"),
    ).toBeInTheDocument();
  });

  it("only enables rollback for committed transactions", async () => {
    const user = userEvent.setup();
    api.listTransactions.mockResolvedValue([
      committedTransaction,
      {
        ...committedTransaction,
        transaction_id: "99999999-9999-9999-9999-999999999999",
        status: "rolled_back",
      },
    ]);
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
