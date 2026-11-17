import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  getStoredSettingValues: vi.fn(),
  setSettingValues: vi.fn(async () => undefined),
}));

vi.mock("~/settings/queries", () => ({
  getStoredSettingValues: mocks.getStoredSettingValues,
  setSettingValues: mocks.setSettingValues,
}));

import { configurePaidSettings } from "./configure-paid-settings";

describe("configurePaidSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("selects hosted STT and never picks a language model", async () => {
    mocks.getStoredSettingValues.mockResolvedValue({
      values: {},
      hasValues: new Set(),
    });

    await configurePaidSettings();

    expect(mocks.setSettingValues).toHaveBeenCalledWith({
      current_stt_provider: "anarlog",
      current_stt_model: "cloud",
    });
  });
});
