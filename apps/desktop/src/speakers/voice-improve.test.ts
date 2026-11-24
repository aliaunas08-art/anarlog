import { beforeEach, describe, expect, it, vi } from "vitest";

import { shouldOfferVoiceImprovement } from "./voice-improve";

const { invokeMock, getStoredSettingValuesMock } = vi.hoisted(() => ({
  invokeMock: vi.fn(),
  getStoredSettingValuesMock: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock }));
vi.mock("~/settings/queries", () => ({
  getStoredSettingValues: getStoredSettingValuesMock,
  setSettingValue: vi.fn(),
}));

function stored(declined: boolean) {
  return {
    hasValues: new Set(["voice_profile_improve_declined"]),
    values: { voice_profile_improve_declined: declined },
  };
}

describe("shouldOfferVoiceImprovement", () => {
  beforeEach(() => vi.clearAllMocks());

  it.each([
    [true, true, true],
    [true, false, true],
    [false, false, true],
    [false, true, false],
  ])(
    "enrolled=%s declined=%s offers=%s",
    async (enrolled, declined, expected) => {
      invokeMock.mockResolvedValue({ enrolled });
      getStoredSettingValuesMock.mockResolvedValue(stored(declined));
      await expect(shouldOfferVoiceImprovement()).resolves.toBe(expected);
      expect(invokeMock).toHaveBeenCalledWith(
        "plugin:transcription|voice_enrollment_status",
      );
    },
  );

  it("does not offer when the backend is unavailable", async () => {
    invokeMock.mockRejectedValue("unknown command");
    getStoredSettingValuesMock.mockResolvedValue(stored(false));
    await expect(shouldOfferVoiceImprovement()).resolves.toBe(false);
  });
});
