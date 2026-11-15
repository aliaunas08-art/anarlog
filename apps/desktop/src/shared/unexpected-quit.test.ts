import { beforeEach, describe, expect, it, vi } from "vitest";

const store = vi.hoisted(() => ({ values: new Map<string, boolean>() }));

vi.mock("@anlg/plugin-store2", () => ({
  commands: {
    getBool: vi.fn(async (scope: string, key: string) => ({
      status: "ok",
      data: store.values.get(`${scope}:${key}`) ?? null,
    })),
    setBool: vi.fn(async (scope: string, key: string, value: boolean) => {
      store.values.set(`${scope}:${key}`, value);
      return { status: "ok", data: null };
    }),
    save: vi.fn(async () => ({ status: "ok", data: null })),
  },
}));

describe("unexpected quit detection", () => {
  beforeEach(() => {
    store.values.clear();
    vi.resetModules();
  });

  it("is false on a first launch and marks the app as running", async () => {
    const { detectUnexpectedQuit } = await import("./unexpected-quit");
    expect(await detectUnexpectedQuit()).toBe(false);
    expect(store.values.get("app-lifecycle:running")).toBe(true);
  });

  it("is true when the previous run never marked a clean exit", async () => {
    store.values.set("app-lifecycle:running", true);
    const { detectUnexpectedQuit } = await import("./unexpected-quit");
    expect(await detectUnexpectedQuit()).toBe(true);
  });

  it("is false after a clean exit", async () => {
    const first = await import("./unexpected-quit");
    await first.detectUnexpectedQuit();
    await first.markCleanExit();
    vi.resetModules();
    const second = await import("./unexpected-quit");
    expect(await second.detectUnexpectedQuit()).toBe(false);
  });
});
