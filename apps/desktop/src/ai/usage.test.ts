import { beforeEach, describe, expect, it } from "vitest";

import {
  getAiUsage,
  isAiUsageLimitReached,
  loadAiUsage,
  recordAiCall,
  recordAskQuery,
  recordAskRedirect,
  resetAiUsageForTests,
  todayKey,
} from "./usage";

describe("ai usage counter", () => {
  beforeEach(() => {
    resetAiUsageForTests();
  });

  it("counts calls per day, task, provider, and Ask scope", () => {
    recordAiCall({ task: "chat", providerId: "anarlog", askScope: "library" });
    recordAiCall({ task: "enhance", providerId: "openai", inputTokens: 5 });
    recordAskRedirect();

    const usage = getAiUsage();
    expect(usage.calls).toBe(2);
    expect(usage.developerKeyCalls).toBe(1);
    expect(usage.byTask).toEqual({ chat: 1, enhance: 1 });
    expect(usage.byProvider).toEqual({ anarlog: 1, openai: 1 });
    expect(usage.byAskScope).toEqual({ library: 1 });
    expect(usage.inputTokens).toBe(5);
    expect(usage.askRedirects).toBe(1);
  });

  it("limits Ask questions on the developer key only, and only when a limit is configured", async () => {
    recordAskQuery({ providerId: "anarlog", askScope: "library" });
    recordAskQuery({ providerId: "anarlog", askScope: "meeting" });
    // Background model calls (summaries, titles) never consume the Ask limit.
    for (let i = 0; i < 5; i += 1) {
      recordAiCall({ task: "enhance", providerId: "anarlog" });
    }
    expect(getAiUsage().askQueriesByScope).toEqual({ library: 1, meeting: 1 });

    await expect(
      isAiUsageLimitReached({ providerId: "anarlog", limit: 2 }),
    ).resolves.toBe(true);
    await expect(
      isAiUsageLimitReached({ providerId: "anarlog", limit: 3 }),
    ).resolves.toBe(false);
    await expect(
      isAiUsageLimitReached({ providerId: "anarlog", limit: null }),
    ).resolves.toBe(false);
    // BYOK records but never blocks.
    await expect(
      isAiUsageLimitReached({ providerId: "openai", limit: 1 }),
    ).resolves.toBe(false);
  });

  it("merges stored counts for the day with calls recorded before the load", async () => {
    const saved: Record<string, string> = {};
    resetAiUsageForTests({
      load: async (date) =>
        JSON.stringify({
          date,
          calls: 4,
          developerKeyCalls: 4,
          byTask: { chat: 4 },
        }),
      save: async (date, json) => {
        saved[date] = json;
      },
    });
    recordAiCall({ task: "chat", providerId: "anarlog" });
    const day = await loadAiUsage();
    expect(day.calls).toBe(5);
    expect(day.byTask).toEqual({ chat: 5 });
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(JSON.parse(saved[todayKey()]!).calls).toBe(5);
  });
});
