import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  appLockSettings: vi.fn(),
  setAppLockIdleMinutes: vi.fn(),
}));

vi.mock("@anlg/plugin-local-auth", () => ({
  commands: {
    appLockSettings: mocks.appLockSettings,
    setAppLockIdleMinutes: mocks.setAppLockIdleMinutes,
  },
}));

import { IdleRelockSetting } from "./idle-relock";

describe("IdleRelockSetting", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    cleanup();
  });

  it("shows the idle time Rust reports (A4.4)", async () => {
    mocks.appLockSettings.mockResolvedValue({
      status: "ok",
      data: { idleMinutes: 15, idleMinuteChoices: [0, 1, 5, 15, 30, 60] },
    });
    render(<IdleRelockSetting disabled={false} />);

    await waitFor(() =>
      expect(screen.getByText("After 15 minutes")).toBeTruthy(),
    );
    expect(screen.getByText("Lock when idle")).toBeTruthy();
    expect(mocks.setAppLockIdleMinutes).not.toHaveBeenCalled();
  });

  it("renders nothing when Rust refuses to say", async () => {
    mocks.appLockSettings.mockResolvedValue({
      status: "error",
      error: "aspc.window_denied",
    });
    const { container } = render(<IdleRelockSetting disabled={false} />);

    await waitFor(() => expect(mocks.appLockSettings).toHaveBeenCalled());
    expect(container.textContent).toBe("");
  });
});
