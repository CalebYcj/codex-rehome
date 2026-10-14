import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readFileSync } from "node:fs";

import { I18nProvider } from "../../lib/i18n";
import UpdateControl from "./UpdateControl";

const updater = vi.hoisted(() => ({
  checkForUpdates: vi.fn(),
  installCheckedUpdate: vi.fn(),
}));

vi.mock("../../lib/updater", () => updater);

const attentionRed = "rgb(180, 35, 24)";
const appStyles = readFileSync("src/App.css", "utf8");
let stylesheet: HTMLStyleElement;
let footer: HTMLElement;

function renderControl(migrationBusy = false) {
  return render(
    <I18nProvider>
      <UpdateControl
        migrationBusy={migrationBusy}
        onInstallingChange={vi.fn()}
        footerContainer={footer}
      />
    </I18nProvider>,
  );
}

function color(selector: string) {
  const element = footer.querySelector(selector);
  if (!element) throw new Error(`Missing update element: ${selector}`);
  return getComputedStyle(element).color;
}

describe("update footer attention colors", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    stylesheet = document.createElement("style");
    // JSDOM lacks CSS variable inheritance. Resolve the real static palette for
    // selector/state regressions; browser verification checks native resolution.
    const root = /^:root\s*\{([^}]+)\}/.exec(appStyles)?.[1] ?? "";
    const tokens = new Map(
      [...root.matchAll(/(--[\w-]+):\s*([^;]+);/g)]
        .map((match) => [match[1], match[2].trim()]),
    );
    stylesheet.textContent = appStyles.replace(
      /var\((--[\w-]+)\)/g, (expression, name: string) => tokens.get(name) ?? expression,
    );
    document.head.append(stylesheet);
    footer = document.createElement("footer");
    document.body.append(footer);
    updater.checkForUpdates.mockResolvedValue({
      status: "available",
      currentVersion: "0.1.34",
      version: "0.1.35",
      notes: "更新提示颜色调整",
    });
  });

  afterEach(() => {
    stylesheet.remove();
    footer.remove();
  });

  it("colors the available label and dot red without recoloring the version or opening a dialog", async () => {
    renderControl();
    await within(footer).findByText("发现更新");

    expect(color(".version-status")).toBe(attentionRed);
    expect(color(".update-dot")).toBe(attentionRed);
    expect(color(".version-number")).toBe("rgb(101, 113, 123)");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(updater.installCheckedUpdate).not.toHaveBeenCalled();
  });

  it("keeps the current-version state neutral", async () => {
    updater.checkForUpdates.mockResolvedValue({ status: "current", currentVersion: "0.1.35" });
    renderControl();
    await within(footer).findByText("当前已是最新版");

    expect(color(".version-status")).not.toBe(attentionRed);
    expect(color(".version-number")).not.toBe(attentionRed);
    expect(footer.querySelector(".update-dot")).toBeNull();
  });

  it("does not apply available-update colors to a failed check", async () => {
    updater.checkForUpdates.mockRejectedValue(new Error("offline"));
    renderControl();
    await within(footer).findByText("检查失败，点击重试");

    expect(color(".version-status")).not.toBe(attentionRed);
    expect(footer.querySelector(".has-update")).toBeNull();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("preserves the migration interlock while showing an available update", async () => {
    renderControl(true);
    const shortcut = await within(footer).findByRole("button", { name: "版本 0.1.34，发现更新" });

    expect(shortcut).toBeDisabled();
    expect(color(".version-status")).toBe(attentionRed);
    expect(updater.installCheckedUpdate).not.toHaveBeenCalled();
  });

  it("opens only the existing confirmation after a user clicks the red update shortcut", async () => {
    const user = userEvent.setup();
    renderControl();
    const shortcut = await within(footer).findByRole("button", { name: "版本 0.1.34，发现更新" });
    await user.click(shortcut);

    expect(screen.getByRole("dialog", { name: "现在更新 ReHome？" })).toBeVisible();
    expect(color(".version-status")).toBe(attentionRed);
    expect(updater.installCheckedUpdate).not.toHaveBeenCalled();
  });
});
