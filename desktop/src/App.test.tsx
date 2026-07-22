import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import App from "./App";

describe("ReHome desktop shell", () => {
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
});
