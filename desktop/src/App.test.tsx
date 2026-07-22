import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "./App";

describe("ReHome desktop shell", () => {
  it.each([
    ["发送", "发送交接"],
    ["接收", "接收交接"],
  ])("moves focus to the %s destination heading", async (actionName, headingName) => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole("button", { name: actionName }));

    const heading = screen.getByRole("heading", { name: headingName });
    expect(heading).toHaveAttribute("tabindex", "-1");
    expect(heading).toHaveFocus();
  });

  it("offers the primary send and receive actions", () => {
    render(<App />);

    expect(screen.getByRole("button", { name: "发送" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "接收" })).toBeInTheDocument();
  });

  it("opens the send view from the primary action", async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole("button", { name: "发送" }));

    expect(screen.getByRole("heading", { name: "发送交接" })).toBeInTheDocument();
  });

  it("moves focus and current-page state through history and home", async () => {
    const user = userEvent.setup();
    render(<App />);

    const historyButton = screen.getByRole("button", { name: "前往历史" });
    const homeButton = screen.getByRole("button", { name: "前往首页" });

    await user.click(historyButton);

    expect(screen.getByRole("heading", { name: "历史记录" })).toHaveFocus();
    expect(historyButton).toHaveAttribute("aria-current", "page");
    expect(homeButton).not.toHaveAttribute("aria-current");

    await user.click(homeButton);

    const homeHeading = screen.getByRole("heading", { name: "迁移工作台" });
    expect(homeHeading).toHaveAttribute("tabindex", "-1");
    expect(homeHeading).toHaveFocus();
    expect(homeButton).toHaveAttribute("aria-current", "page");
    expect(historyButton).not.toHaveAttribute("aria-current");
  });
});
