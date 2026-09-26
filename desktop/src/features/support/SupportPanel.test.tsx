import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { I18nProvider } from "../../lib/i18n";
import * as api from "../../lib/api";
import SupportPanel from "./SupportPanel";

vi.mock("../../lib/api", () => ({ prepareSupport: vi.fn(), copySupportText: vi.fn(), openSupportIssue: vi.fn(), recheckSupport: vi.fn(), openPath: vi.fn() }));
const preview = { support_id: "incident", codex_text: "ReHome private evidence", github_text: "ReHome public summary", saved: true, reveal_id: "reveal", can_recheck: true };
function setup() { render(<I18nProvider><SupportPanel source={{ kind: "incident", support_id: "incident" }} /></I18nProvider>); return userEvent.setup(); }
beforeEach(() => {
  vi.resetAllMocks(); localStorage.clear();
  vi.mocked(api.prepareSupport).mockResolvedValue(preview);
  vi.mocked(api.copySupportText).mockResolvedValue();
  vi.mocked(api.openSupportIssue).mockResolvedValue("opened");
});
describe("support handoff", () => {
  it("requires an actual failed-chat confirmation for completed imports", async () => {
    render(<I18nProvider><SupportPanel source={{ kind: "transaction", transaction_id: "successful-import" }} /></I18nProvider>);
    const user = userEvent.setup();
    await user.click(screen.getByText("会话打不开？获取帮助"));
    expect(screen.getByText("复制到 Codex")).toBeDisabled();
    expect(api.prepareSupport).not.toHaveBeenCalled();
    await user.click(screen.getByRole("checkbox"));
    await user.click(screen.getByText("复制到 Codex"));
    expect(api.prepareSupport).toHaveBeenCalledWith({ kind: "transaction", transaction_id: "successful-import" }, "zh-CN", "", true);
    await screen.findByDisplayValue(preview.codex_text);
    await user.click(screen.getByRole("checkbox"));
    expect(screen.queryByDisplayValue(preview.codex_text)).toBeNull();
    expect(screen.getByText("复制到 Codex")).toBeDisabled();
  });
  it("requires preview and confirmation before copying or opening GitHub", async () => {
    const user = setup();
    expect(api.prepareSupport).not.toHaveBeenCalled();
    await user.click(screen.getByText("需要帮助？"));
    await user.click(screen.getByText("到 GitHub 提交问题"));
    expect(await screen.findByDisplayValue(preview.github_text)).toBeVisible();
    expect(api.openSupportIssue).not.toHaveBeenCalled();
    expect(api.copySupportText).not.toHaveBeenCalled();
    await user.click(screen.getByText("打开 GitHub"));
    expect(await screen.findByText("已打开提交页面，请在 GitHub 确认提交。")).toBeVisible();
    expect(api.openSupportIssue).toHaveBeenCalledWith("incident", "zh-CN");
  });
  it("does not carry a failure confirmation to another transaction", async () => {
    localStorage.setItem("rehome.locale", "en");
    const view = render(<I18nProvider><SupportPanel source={{ kind: "transaction", transaction_id: "first" }} /></I18nProvider>);
    const user = userEvent.setup();
    await user.click(screen.getByText("Can't open a chat? Get help"));
    await user.click(screen.getByRole("checkbox"));
    expect(screen.getByText("Copy for Codex")).toBeEnabled();
    view.rerender(<I18nProvider><SupportPanel source={{ kind: "transaction", transaction_id: "second" }} /></I18nProvider>);
    expect(screen.getByRole("checkbox")).not.toBeChecked();
    expect(screen.getByText("Copy for Codex")).toBeDisabled();
  });
  it("keeps selectable text and does not claim success when clipboard fails", async () => {
    vi.mocked(api.copySupportText).mockRejectedValue(new Error("clipboard unavailable"));
    const user = setup();
    await user.click(screen.getByText("需要帮助？"));
    await user.click(screen.getByText("复制到 Codex"));
    await screen.findByDisplayValue(preview.codex_text);
    await user.click(screen.getByText("确认复制"));
    expect(await screen.findByRole("alert")).toHaveTextContent("clipboard unavailable");
    expect(screen.queryByText("已复制，请到本机 Codex 新对话粘贴。")).toBeNull();
    expect(screen.getByDisplayValue(preview.codex_text)).toBeVisible();
  });
  it("does not call a basic recheck a repaired conversation", async () => {
    vi.mocked(api.recheckSupport).mockResolvedValue({ checked_at: "now", omitted_sessions: 1, checks: [{ subject: "database", code: "database_not_checked", status: "unknown" }] });
    const user = setup();
    await user.click(screen.getByText("需要帮助？"));
    await user.click(screen.getByText("复制到 Codex"));
    await user.click(await screen.findByText("重新检查数据"));
    expect(await screen.findByText(/本次未检查数据库/)).toBeVisible();
    expect(screen.getByText(/数据库及实际续聊尚未验证/)).toBeVisible();
  });
  it("renders English and discards stale preparation after switching incidents", async () => {
    localStorage.setItem("rehome.locale", "en");
    let finish!: (value: typeof preview) => void;
    vi.mocked(api.prepareSupport).mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    const view = render(<I18nProvider><SupportPanel source={{ kind: "incident", support_id: "first" }} /></I18nProvider>);
    const user = userEvent.setup();
    await user.click(screen.getByText("Need help?"));
    await user.type(screen.getByRole("textbox"), "old private note");
    await user.click(screen.getByText("Copy for Codex"));
    view.rerender(<I18nProvider><SupportPanel source={{ kind: "incident", support_id: "second" }} /></I18nProvider>);
    expect(screen.getByRole("textbox")).toHaveValue("");
    finish(preview);
    await waitFor(() => expect(screen.queryByDisplayValue(preview.codex_text)).toBeNull());
  });
});
