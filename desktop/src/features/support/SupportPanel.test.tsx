import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../lib/i18n";
import * as api from "../../lib/api";
import SupportPanel from "./SupportPanel";

vi.mock("../../lib/api", () => ({ prepareSupport: vi.fn(), copySupportText: vi.fn(), openSupportIssue: vi.fn(), recheckSupport: vi.fn(), openPath: vi.fn() }));
const preview = { support_id: "incident", codex_text: "ReHome private evidence", github_text: "ReHome public summary", saved: true, reveal_id: "reveal", can_recheck: true };
function incident() { render(<I18nProvider><SupportPanel source={{ kind: "incident", support_id: "incident" }} /></I18nProvider>); return userEvent.setup(); }

beforeEach(() => {
  vi.resetAllMocks();
  localStorage.clear();
  vi.mocked(api.prepareSupport).mockResolvedValue(preview);
  vi.mocked(api.copySupportText).mockResolvedValue();
  vi.mocked(api.openSupportIssue).mockResolvedValue("opened");
});

describe("support handoff", () => {
  it("opens a prominent incident solution and prepares Codex text without a help click", async () => {
    incident();
    expect(await screen.findByText("操作未完成，已备好 Codex 排查内容")).toBeVisible();
    expect(screen.getByRole("button", { name: "收起" })).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByDisplayValue(preview.codex_text)).toBeVisible();
    expect(api.prepareSupport).toHaveBeenCalledExactlyOnceWith({ kind: "incident", support_id: "incident" }, "zh-CN", "", false);
    expect(api.copySupportText).not.toHaveBeenCalled();
    expect(api.openSupportIssue).not.toHaveBeenCalled();
  });

  it("keeps the public GitHub path optional and never posts automatically", async () => {
    const user = incident();
    await screen.findByDisplayValue(preview.codex_text);
    await user.click(screen.getByRole("button", { name: "预览 GitHub 问题" }));
    expect(await screen.findByDisplayValue(preview.github_text)).toBeVisible();
    expect(api.openSupportIssue).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "返回 Codex 求助内容" }));
    expect(screen.getByDisplayValue(preview.codex_text)).toBeVisible();
    expect(api.openSupportIssue).not.toHaveBeenCalled();
  });

  it("requires failure confirmation for a completed import", async () => {
    render(<I18nProvider><SupportPanel source={{ kind: "transaction", transaction_id: "successful-import" }} /></I18nProvider>);
    const user = userEvent.setup();
    expect(api.prepareSupport).not.toHaveBeenCalled();
    await user.click(screen.getByText("项目或对话找不到、打不开？获取帮助"));
    expect(screen.getByText("生成给 Codex 的求助内容")).toBeDisabled();
    await user.click(screen.getByRole("checkbox"));
    await user.click(screen.getByText("生成给 Codex 的求助内容"));
    expect(api.prepareSupport).toHaveBeenCalledWith({ kind: "transaction", transaction_id: "successful-import" }, "zh-CN", "", true);
    await screen.findByDisplayValue(preview.codex_text);
    await user.click(screen.getByRole("checkbox"));
    expect(screen.queryByDisplayValue(preview.codex_text)).toBeNull();
    expect(screen.getByText("生成给 Codex 的求助内容")).toBeDisabled();
  });

  it("does not carry a failure confirmation to another transaction", async () => {
    localStorage.setItem("rehome.locale", "en");
    const view = render(<I18nProvider><SupportPanel source={{ kind: "transaction", transaction_id: "first" }} /></I18nProvider>);
    const user = userEvent.setup();
    await user.click(screen.getByText("Can't find or open a project or chat? Get help"));
    await user.click(screen.getByRole("checkbox"));
    expect(screen.getByText("Prepare help for Codex")).toBeEnabled();
    view.rerender(<I18nProvider><SupportPanel source={{ kind: "transaction", transaction_id: "second" }} /></I18nProvider>);
    await waitFor(() => expect(screen.getByRole("button", { name: "Can't find or open a project or chat? Get help" })).toHaveAttribute("aria-expanded", "false"));
    await user.click(screen.getByText("Can't find or open a project or chat? Get help"));
    expect(screen.getByRole("checkbox")).not.toBeChecked();
    expect(screen.getByText("Prepare help for Codex")).toBeDisabled();
  });

  it("makes note edits invalidate the preview and offers a visible regenerate action", async () => {
    const user = incident();
    await screen.findByDisplayValue(preview.codex_text);
    await user.click(screen.getByText("补充情况（可选，仅用于本机求助）"));
    await user.type(screen.getByPlaceholderText(/左栏找不到项目/), "打不开旧对话");
    expect(screen.queryByDisplayValue(preview.codex_text)).toBeNull();
    expect(screen.getByText("补充情况已更改，请重新生成后再复制。")).toBeVisible();
    expect(screen.queryByRole("button", { name: "复制给 Codex" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "重新生成给 Codex 的求助内容" }));
    expect(api.prepareSupport).toHaveBeenLastCalledWith({ kind: "incident", support_id: "incident" }, "zh-CN", "打不开旧对话", false);
    expect(await screen.findByDisplayValue(preview.codex_text)).toBeVisible();
  });

  it("keeps selectable text and does not claim success when clipboard fails", async () => {
    vi.mocked(api.copySupportText).mockRejectedValue(new Error("clipboard unavailable"));
    const user = incident();
    await screen.findByDisplayValue(preview.codex_text);
    await user.click(screen.getByRole("button", { name: "复制给 Codex" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("clipboard unavailable");
    expect(screen.queryByText(/已复制。打开本机 Codex/)).toBeNull();
    expect(screen.getByDisplayValue(preview.codex_text)).toBeVisible();
  });

  it("allows retry if automatic preparation fails", async () => {
    vi.mocked(api.prepareSupport).mockRejectedValueOnce(new Error("diagnostic unavailable")).mockResolvedValue(preview);
    const user = incident();
    expect(await screen.findByRole("alert")).toHaveTextContent("diagnostic unavailable");
    await user.click(screen.getByRole("button", { name: "重新生成给 Codex 的求助内容" }));
    expect(await screen.findByDisplayValue(preview.codex_text)).toBeVisible();
  });

  it("does not call a basic recheck a repaired conversation", async () => {
    vi.mocked(api.recheckSupport).mockResolvedValue({ checked_at: "now", omitted_sessions: 1, checks: [{ subject: "database", code: "database_not_checked", status: "unknown" }] });
    const user = incident();
    await screen.findByDisplayValue(preview.codex_text);
    await user.click(screen.getByText("重新检查数据"));
    expect(await screen.findByText(/本次未检查数据库/)).toBeVisible();
    expect(screen.getByText(/数据库及实际续聊尚未验证/)).toBeVisible();
  });

  it("renders English and discards stale preparation after switching incidents", async () => {
    localStorage.setItem("rehome.locale", "en");
    let finishFirst!: (value: typeof preview) => void;
    vi.mocked(api.prepareSupport).mockImplementationOnce(() => new Promise(resolve => { finishFirst = resolve; }));
    const view = render(<I18nProvider><SupportPanel source={{ kind: "incident", support_id: "first" }} /></I18nProvider>);
    expect(screen.getByText("Preparing details from this failure…")).toBeVisible();
    view.rerender(<I18nProvider><SupportPanel source={{ kind: "incident", support_id: "second" }} /></I18nProvider>);
    await screen.findByDisplayValue(preview.codex_text);
    finishFirst({ ...preview, codex_text: "stale evidence" });
    await waitFor(() => expect(screen.queryByDisplayValue("stale evidence")).toBeNull());
    expect(api.prepareSupport).toHaveBeenLastCalledWith({ kind: "incident", support_id: "second" }, "en", "", false);
  });
});
