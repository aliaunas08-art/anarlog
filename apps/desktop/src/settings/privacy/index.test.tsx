import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  setSettingValues: vi.fn(),
  setLockEnabled: vi.fn(),
  refreshAvailability: vi.fn(),
  lockEnabled: false,
  available: true as boolean | null,
  authenticating: false,
  platform: "macos" as string,
  values: {
    telemetry_consent: true,
    crash_reporting_consent: false,
    lock_app: false,
  },
}));

vi.mock("@tauri-apps/plugin-os", () => ({
  platform: () => mocks.platform,
}));

vi.mock("~/settings/queries", () => ({
  useSetSettingValues: () => mocks.setSettingValues,
  useStoredSettingValuesQuery: () => ({
    data: {
      values: mocks.values,
      hasValues: new Set([
        "telemetry_consent",
        "crash_reporting_consent",
        "lock_app",
      ]),
    },
    isLoading: false,
    error: null,
  }),
}));

vi.mock("~/lock/store", () => ({
  startRustLockSync: vi.fn(() => Promise.resolve()),
  useAppLock: (selector: (state: typeof mocks) => unknown) =>
    selector({
      available: mocks.available,
      authenticating: mocks.authenticating,
      lockEnabled: mocks.lockEnabled,
      setLockEnabled: mocks.setLockEnabled,
      refreshAvailability: mocks.refreshAvailability,
    } as never),
}));

import { SettingsPrivacy } from ".";

describe("SettingsPrivacy", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.values.telemetry_consent = true;
    mocks.values.crash_reporting_consent = false;
    mocks.values.lock_app = false;
    mocks.available = true;
    mocks.authenticating = false;
    mocks.platform = "macos";
    mocks.lockEnabled = false;
    mocks.setLockEnabled.mockResolvedValue(true);
    mocks.refreshAvailability.mockResolvedValue(true);
  });

  afterEach(cleanup);

  it("controls usage data and error reporting independently", () => {
    render(<SettingsPrivacy />);

    const posthog = screen.getByRole("switch", {
      name: "Share usage data",
    });
    const errorReporting = screen.getByRole("switch", { name: "Error" });

    expect(posthog.getAttribute("data-state")).toBe("checked");
    expect(errorReporting.getAttribute("data-state")).toBe("unchecked");

    fireEvent.click(posthog);
    fireEvent.click(errorReporting);

    expect(mocks.setSettingValues).toHaveBeenNthCalledWith(1, {
      telemetry_consent: false,
    });
    expect(mocks.setSettingValues).toHaveBeenNthCalledWith(2, {
      crash_reporting_consent: true,
    });
  });

  it("asks Rust to turn the lock on and never writes the setting itself", async () => {
    render(<SettingsPrivacy />);

    fireEvent.click(screen.getByRole("switch", { name: "Lock app" }));

    await waitFor(() => {
      expect(mocks.setLockEnabled).toHaveBeenCalledWith(true);
    });
    expect(mocks.setSettingValues).not.toHaveBeenCalled();
  });

  it("shows the lock state reported by Rust", () => {
    mocks.lockEnabled = true;
    render(<SettingsPrivacy />);

    expect(
      screen
        .getByRole("switch", { name: "Lock app" })
        .getAttribute("data-state"),
    ).toBe("checked");
  });
});
