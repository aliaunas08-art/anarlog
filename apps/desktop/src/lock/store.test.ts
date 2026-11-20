import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  authenticateDevice: vi.fn(),
  isDeviceAuthAvailable: vi.fn(),
  getRustLockState: vi.fn(),
  unlockAppInRust: vi.fn(),
  lockAppInRust: vi.fn(),
  setAppLockEnabledInRust: vi.fn(),
  listenRustLockState: vi.fn(),
  flush: vi.fn(),
}));

vi.mock("./auth", () => ({
  authenticateDevice: mocks.authenticateDevice,
  isDeviceAuthAvailable: mocks.isDeviceAuthAvailable,
  getRustLockState: mocks.getRustLockState,
  unlockAppInRust: mocks.unlockAppInRust,
  lockAppInRust: mocks.lockAppInRust,
  setAppLockEnabledInRust: mocks.setAppLockEnabledInRust,
  listenRustLockState: mocks.listenRustLockState,
}));

vi.mock("~/db/write-queue", () => ({
  flushDatabaseWritesWithin: mocks.flush,
}));

import { isLockedFlag } from "./flag";
import { useAppLock, waitForAppUnlock } from "./store";

describe("isLockedFlag", () => {
  it("treats sqlite integers and booleans as locked", () => {
    expect(isLockedFlag(1)).toBe(true);
    expect(isLockedFlag(true)).toBe(true);
    expect(isLockedFlag(0)).toBe(false);
    expect(isLockedFlag(false)).toBe(false);
    expect(isLockedFlag(null)).toBe(false);
  });
});

describe("app lock store", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.authenticateDevice.mockResolvedValue(true);
    mocks.flush.mockResolvedValue(undefined);
    mocks.listenRustLockState.mockResolvedValue(() => {});
    mocks.getRustLockState.mockResolvedValue({
      enabled: true,
      unlocked: false,
    });
    mocks.lockAppInRust.mockResolvedValue({ enabled: true, unlocked: false });
    useAppLock.setState({
      available: null,
      authenticating: false,
      lockEnabled: true,
      appUnlocked: false,
      unlockError: null,
      revealedNoteIds: {},
    });
  });

  it("reveals a note after authentication without unlocking the app", async () => {
    await expect(
      useAppLock.getState().revealNote("session-1", "open"),
    ).resolves.toBe(true);
    expect(useAppLock.getState().isNoteRevealed("session-1")).toBe(true);
    expect(useAppLock.getState().appUnlocked).toBe(false);
    expect(mocks.unlockAppInRust).not.toHaveBeenCalled();
  });

  it("unlocks only when Rust reports unlocked", async () => {
    mocks.unlockAppInRust.mockResolvedValue({
      ok: true,
      state: { enabled: true, unlocked: false },
    });
    await expect(useAppLock.getState().unlockApp()).resolves.toBe(false);
    expect(useAppLock.getState().appUnlocked).toBe(false);

    mocks.unlockAppInRust.mockResolvedValue({
      ok: true,
      state: { enabled: true, unlocked: true },
    });
    await expect(useAppLock.getState().unlockApp()).resolves.toBe(true);
    expect(useAppLock.getState().appUnlocked).toBe(true);
    expect(mocks.authenticateDevice).not.toHaveBeenCalled();
  });

  it("keeps the Rust error when authentication is unavailable", async () => {
    mocks.unlockAppInRust.mockResolvedValue({
      ok: false,
      error: "aspc.auth_unavailable: no",
    });
    await expect(useAppLock.getState().unlockApp()).resolves.toBe(false);
    expect(useAppLock.getState().appUnlocked).toBe(false);
    expect(useAppLock.getState().unlockError).toContain(
      "aspc.auth_unavailable",
    );
  });

  it("flushes writes, then asks Rust to lock, and clears revealed notes", async () => {
    useAppLock.setState({
      appUnlocked: true,
      revealedNoteIds: { "session-1": true },
    });
    const order: string[] = [];
    mocks.flush.mockImplementation(async () => {
      order.push("flush");
    });
    mocks.lockAppInRust.mockImplementation(async () => {
      order.push("lock");
      return { enabled: true, unlocked: false };
    });
    await useAppLock.getState().lockApp();
    expect(order).toEqual(["flush", "lock"]);
    expect(useAppLock.getState().isNoteRevealed("session-1")).toBe(false);
    expect(useAppLock.getState().appUnlocked).toBe(false);
  });

  it("does nothing on lock when the lock is off", async () => {
    useAppLock.setState({ lockEnabled: false, appUnlocked: true });
    await useAppLock.getState().lockApp();
    expect(mocks.lockAppInRust).not.toHaveBeenCalled();
    expect(useAppLock.getState().appUnlocked).toBe(true);
  });

  it("ignores a note reveal that finishes after the app locked", async () => {
    let resolveAuth!: (value: boolean) => void;
    mocks.authenticateDevice.mockReturnValue(
      new Promise<boolean>((resolve) => {
        resolveAuth = resolve;
      }),
    );
    useAppLock.setState({ appUnlocked: true });
    const pending = useAppLock.getState().revealNote("session-1", "open");
    useAppLock.getState().applyRustState({ enabled: true, unlocked: false });
    resolveAuth(true);
    await expect(pending).resolves.toBe(false);
    expect(useAppLock.getState().isNoteRevealed("session-1")).toBe(false);
  });

  it("waits for Rust to unlock before resolving", async () => {
    let resolved = false;
    const waiting = waitForAppUnlock().then(() => {
      resolved = true;
    });
    await Promise.resolve();
    expect(resolved).toBe(false);
    useAppLock.getState().applyRustState({ enabled: true, unlocked: true });
    await waiting;
    expect(resolved).toBe(true);
  });
});
