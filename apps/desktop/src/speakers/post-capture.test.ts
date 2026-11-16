import { describe, expect, it, vi } from "vitest";

import {
  resolveSpeakerAttributionMode,
  runPostCaptureSpeakerPass,
} from "./post-capture";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const result = {
  diarizedChannels: [1],
  speakerCount: 3,
  selfMatchScore: null,
  selfMatched: false,
  uncertainWords: 0,
  skippedReason: null,
};

describe("resolveSpeakerAttributionMode", () => {
  it("uses in_person for mic-only captures", () => {
    expect(
      resolveSpeakerAttributionMode({ inPerson: true, recovered: false }),
    ).toBe("in_person");
    expect(
      resolveSpeakerAttributionMode({ inPerson: true, recovered: true }),
    ).toBe("in_person");
  });

  it("uses online for meeting captures and auto after a restart", () => {
    expect(
      resolveSpeakerAttributionMode({ inPerson: false, recovered: false }),
    ).toBe("online");
    expect(
      resolveSpeakerAttributionMode({ inPerson: false, recovered: true }),
    ).toBe("auto");
  });
});

describe("runPostCaptureSpeakerPass", () => {
  it("calls the attribution command with the session, transcript and mode", async () => {
    const attribute = vi.fn().mockResolvedValue(result);
    await expect(
      runPostCaptureSpeakerPass({
        sessionId: "s1",
        transcriptId: "t1",
        mode: "in_person",
        attribute,
      }),
    ).resolves.toEqual(result);
    expect(attribute).toHaveBeenCalledWith({
      sessionId: "s1",
      transcriptId: "t1",
      mode: "in_person",
    });
  });

  it("swallows failures so saving and the summary continue", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const attribute = vi.fn().mockRejectedValue("model missing");
    await expect(
      runPostCaptureSpeakerPass({
        sessionId: "s1",
        transcriptId: "t1",
        mode: "online",
        attribute,
      }),
    ).resolves.toBeNull();
    warn.mockRestore();
  });

  it("gives up after the timeout instead of holding the summary", async () => {
    vi.useFakeTimers();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const attribute = vi.fn(() => new Promise<typeof result>(() => {}));
    const pass = runPostCaptureSpeakerPass({
      sessionId: "s1",
      transcriptId: "t1",
      mode: "online",
      attribute,
      timeoutMs: 1_000,
    });
    await vi.advanceTimersByTimeAsync(1_000);
    await expect(pass).resolves.toBeNull();
    warn.mockRestore();
    vi.useRealTimers();
  });
});
