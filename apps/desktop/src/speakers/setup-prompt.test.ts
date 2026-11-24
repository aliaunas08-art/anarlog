import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  maybeShowSpeakerSetupPrompt,
  type SpeakerSetupPromptDeps,
} from "./setup-prompt";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("~/db", () => ({ liveQueryClient: { execute: vi.fn() } }));
vi.mock("~/settings/queries", () => ({
  getStoredSettingValues: vi.fn(),
  setSettingValue: vi.fn(),
}));
vi.mock("~/store/zustand/tabs", () => ({ useTabs: { getState: vi.fn() } }));
vi.mock("@anlg/ui/components/ui/toast", () => ({ toast: vi.fn() }));

function createDeps(
  overrides: Partial<SpeakerSetupPromptDeps> = {},
): SpeakerSetupPromptDeps & { shown: { value: boolean } } {
  const shown = { value: false };
  return {
    shown,
    wasShown: vi.fn(async () => shown.value),
    markShown: vi.fn(async () => {
      shown.value = true;
    }),
    getProfileName: vi.fn(async () => ""),
    isEnrolled: vi.fn(async () => false),
    show: vi.fn(),
    openSettings: vi.fn(),
    ...overrides,
  };
}

describe("maybeShowSpeakerSetupPrompt", () => {
  beforeEach(() => vi.clearAllMocks());

  it("shows once when the name or voice profile is missing", async () => {
    const deps = createDeps();

    await expect(maybeShowSpeakerSetupPrompt("user-1", deps)).resolves.toBe(
      true,
    );
    await expect(maybeShowSpeakerSetupPrompt("user-1", deps)).resolves.toBe(
      false,
    );

    expect(deps.show).toHaveBeenCalledTimes(1);
    expect(deps.markShown).toHaveBeenCalledTimes(1);
  });

  it("opens settings from the prompt action", async () => {
    const deps = createDeps({
      show: vi.fn((open: () => void) => open()),
    });
    await maybeShowSpeakerSetupPrompt("user-1", deps);
    expect(deps.openSettings).toHaveBeenCalledTimes(1);
  });

  it("shows when only the voice profile is missing", async () => {
    const deps = createDeps({ getProfileName: vi.fn(async () => "Ana") });
    await expect(maybeShowSpeakerSetupPrompt("user-1", deps)).resolves.toBe(
      true,
    );
  });

  it("stays quiet when both are set up, and never asks later", async () => {
    const deps = createDeps({
      getProfileName: vi.fn(async () => "Ana"),
      isEnrolled: vi.fn(async () => true),
    });
    await expect(maybeShowSpeakerSetupPrompt("user-1", deps)).resolves.toBe(
      false,
    );
    expect(deps.show).not.toHaveBeenCalled();
    expect(deps.shown.value).toBe(true);
  });

  it("never throws when the settings store fails", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const deps = createDeps({
      wasShown: vi.fn(async () => {
        throw new Error("db closed");
      }),
    });
    await expect(maybeShowSpeakerSetupPrompt("user-1", deps)).resolves.toBe(
      false,
    );
    expect(deps.show).not.toHaveBeenCalled();
    warn.mockRestore();
  });
});
