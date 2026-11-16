import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => {
  // A stand-in for the Rust lock (crates/aspc/src/lock.rs): only a device
  // authentication run "in Rust" unlocks, and a lock during the prompt wins.
  const rust = { enabled: true, unlocked: false, epoch: 0 };
  const deviceAuth = vi.fn();
  const snapshot = () => ({ enabled: rust.enabled, unlocked: rust.unlocked });
  return {
    rust,
    settingsReady: true,
    deviceAuth,
    getCurrentWebviewWindowLabel: vi.fn(() => "main"),
    visibilityListen: vi.fn(),
    isVisible: vi.fn(),
    restartLiveQueries: vi.fn(),
    invalidateQueries: vi.fn(),
    unlockAppInRust: vi.fn(async () => {
      const epoch = rust.epoch;
      const ok = await deviceAuth("open");
      if (ok && epoch === rust.epoch && rust.enabled) rust.unlocked = true;
      return { ok: true as const, state: snapshot() };
    }),
    lockAppInRust: vi.fn(async () => {
      if (rust.enabled) {
        rust.epoch += 1;
        rust.unlocked = false;
      }
      return snapshot();
    }),
    getRustLockState: vi.fn(async () => snapshot()),
  };
});

vi.mock("@tauri-apps/api/webviewWindow", () => ({
  getCurrentWebviewWindow: () => ({ isVisible: mocks.isVisible }),
}));

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({ invalidateQueries: mocks.invalidateQueries }),
}));

vi.mock("@lingui/react/macro", () => ({
  Trans: ({ children }: { children?: unknown }) => children,
  useLingui: () => ({
    t: (strings: TemplateStringsArray) => strings[0],
  }),
}));

vi.mock("@tauri-apps/plugin-os", () => ({
  platform: () => "macos",
}));

vi.mock("@anlg/plugin-windows", () => ({
  events: {
    visibilityEvent: {
      listen: mocks.visibilityListen,
    },
  },
  getCurrentWebviewWindowLabel: mocks.getCurrentWebviewWindowLabel,
}));

vi.mock("~/settings/queries", () => ({
  useSettingsReady: () => mocks.settingsReady,
}));

vi.mock("~/db", () => ({
  restartLiveQueries: mocks.restartLiveQueries,
}));

vi.mock("~/db/write-queue", () => ({
  flushDatabaseWritesWithin: vi.fn(() => Promise.resolve()),
}));

vi.mock("./auth", () => ({
  DEVICE_AUTH_REASON: {
    openApp: "open",
    lockNote: "lock this note",
    unlockNote: "unlock this note",
    changeLockSettings: "change lock settings",
  },
  authenticateDevice: vi.fn(),
  isDeviceAuthAvailable: vi.fn(() => Promise.resolve(true)),
  getRustLockState: mocks.getRustLockState,
  unlockAppInRust: mocks.unlockAppInRust,
  lockAppInRust: mocks.lockAppInRust,
  setAppLockEnabledInRust: vi.fn(),
  listenRustLockState: vi.fn(() => Promise.resolve(() => {})),
}));

import { AppLockGate } from "./gate";
import { useAppLock } from "./store";

type VisibilityPayload = {
  window: { type: string };
  visible: boolean;
};

function getVisibilityHandler() {
  const calls = mocks.visibilityListen.mock.calls;
  const listener = calls[calls.length - 1]?.[0] as
    | ((event: { payload: VisibilityPayload }) => void)
    | undefined;
  if (!listener) {
    throw new Error("visibility listener was not registered");
  }
  return listener;
}

async function renderLockedGate() {
  render(
    <AppLockGate>
      <div>app content</div>
    </AppLockGate>,
  );

  await waitFor(() => {
    expect(mocks.deviceAuth).toHaveBeenCalledWith("open");
    expect(useAppLock.getState().appUnlocked).toBe(true);
    expect(screen.getByText("app content")).toBeTruthy();
  });
}

describe("AppLockGate", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mocks.settingsReady = true;
    mocks.rust.enabled = true;
    mocks.rust.unlocked = false;
    mocks.getCurrentWebviewWindowLabel.mockReturnValue("main");
    mocks.deviceAuth.mockResolvedValue(true);
    mocks.visibilityListen.mockResolvedValue(vi.fn());
    mocks.isVisible.mockResolvedValue(true);
    useAppLock.setState({
      available: true,
      authenticating: false,
      lockEnabled: true,
      appUnlocked: false,
      unlockError: null,
      revealedNoteIds: {},
    });
  });

  afterEach(cleanup);

  it("does not lock or prompt when the window stays open", async () => {
    await renderLockedGate();

    expect(mocks.deviceAuth).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Notes is Locked")).toBeNull();
  });

  it("waits for a hidden main window to open before prompting", async () => {
    mocks.isVisible.mockResolvedValue(false);
    render(
      <AppLockGate>
        <div>app content</div>
      </AppLockGate>,
    );

    await waitFor(() => {
      expect(mocks.isVisible).toHaveBeenCalled();
    });
    expect(mocks.deviceAuth).not.toHaveBeenCalled();

    const emit = getVisibilityHandler();
    await act(async () => {
      emit({ payload: { window: { type: "main" }, visible: true } });
    });

    await waitFor(() => {
      expect(mocks.deviceAuth).toHaveBeenCalledWith("open");
      expect(useAppLock.getState().appUnlocked).toBe(true);
    });
  });

  it("does not prompt when the main window closes before visibility resolves", async () => {
    let resolveVisible!: (value: boolean) => void;
    mocks.isVisible.mockReturnValue(
      new Promise<boolean>((resolve) => {
        resolveVisible = resolve;
      }),
    );
    render(
      <AppLockGate>
        <div>app content</div>
      </AppLockGate>,
    );
    await waitFor(() => {
      expect(mocks.visibilityListen).toHaveBeenCalled();
    });
    const emit = getVisibilityHandler();
    await act(async () => {
      emit({ payload: { window: { type: "main" }, visible: false } });
    });
    await act(async () => {
      resolveVisible(true);
    });

    expect(mocks.deviceAuth).not.toHaveBeenCalled();
    expect(useAppLock.getState().appUnlocked).toBe(false);
  });

  it("prompts in a note window without checking main visibility", async () => {
    mocks.getCurrentWebviewWindowLabel.mockReturnValue("note-1");
    mocks.isVisible.mockResolvedValue(false);
    render(
      <AppLockGate>
        <div>app content</div>
      </AppLockGate>,
    );

    await waitFor(() => {
      expect(mocks.deviceAuth).toHaveBeenCalledWith("open");
    });
  });

  it("locks on main window close without prompting", async () => {
    await renderLockedGate();
    const emit = getVisibilityHandler();

    await act(async () => {
      emit({ payload: { window: { type: "main" }, visible: false } });
    });

    expect(useAppLock.getState().appUnlocked).toBe(false);
    expect(mocks.deviceAuth).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Notes is Locked")).toBeTruthy();
  });

  it("prompts only after the closed window is opened again", async () => {
    await renderLockedGate();
    const emit = getVisibilityHandler();

    await act(async () => {
      emit({ payload: { window: { type: "main" }, visible: false } });
    });

    expect(mocks.deviceAuth).toHaveBeenCalledTimes(1);
    expect(screen.getByText("Notes is Locked")).toBeTruthy();

    await act(async () => {
      emit({ payload: { window: { type: "main" }, visible: true } });
    });

    await waitFor(() => {
      expect(mocks.deviceAuth).toHaveBeenCalledTimes(2);
      expect(useAppLock.getState().appUnlocked).toBe(true);
    });
    expect(screen.queryByText("Notes is Locked")).toBeNull();
  });

  it("reprompts on reopen if close interrupted an in-flight prompt", async () => {
    let resolveAuth!: (value: boolean) => void;
    mocks.deviceAuth.mockReturnValue(
      new Promise<boolean>((resolve) => {
        resolveAuth = resolve;
      }),
    );

    render(
      <AppLockGate>
        <div>app content</div>
      </AppLockGate>,
    );

    await waitFor(() => {
      expect(mocks.deviceAuth).toHaveBeenCalledTimes(1);
      expect(useAppLock.getState().authenticating).toBe(true);
    });

    const emit = getVisibilityHandler();
    await act(async () => {
      emit({ payload: { window: { type: "main" }, visible: false } });
    });
    await act(async () => {
      emit({ payload: { window: { type: "main" }, visible: true } });
    });

    expect(mocks.deviceAuth).toHaveBeenCalledTimes(1);
    expect(useAppLock.getState().appUnlocked).toBe(false);
    expect(screen.getByText("Notes is Locked")).toBeTruthy();

    mocks.deviceAuth.mockResolvedValue(true);
    await act(async () => {
      resolveAuth(true);
    });

    await waitFor(() => {
      expect(mocks.deviceAuth).toHaveBeenCalledTimes(2);
      expect(useAppLock.getState().appUnlocked).toBe(true);
    });
    expect(screen.queryByText("Notes is Locked")).toBeNull();
  });

  it("does not unlock from a prompt that finishes after close", async () => {
    let resolveAuth!: (value: boolean) => void;
    mocks.deviceAuth.mockReturnValue(
      new Promise<boolean>((resolve) => {
        resolveAuth = resolve;
      }),
    );

    render(
      <AppLockGate>
        <div>app content</div>
      </AppLockGate>,
    );

    await waitFor(() => {
      expect(mocks.deviceAuth).toHaveBeenCalledTimes(1);
      expect(useAppLock.getState().authenticating).toBe(true);
    });

    const emit = getVisibilityHandler();
    await act(async () => {
      emit({ payload: { window: { type: "main" }, visible: false } });
    });

    await act(async () => {
      resolveAuth(true);
    });

    await waitFor(() => {
      expect(useAppLock.getState().authenticating).toBe(false);
    });
    expect(useAppLock.getState().appUnlocked).toBe(false);
    expect(screen.getByText("Notes is Locked")).toBeTruthy();

    mocks.deviceAuth.mockResolvedValue(true);
    await act(async () => {
      emit({ payload: { window: { type: "main" }, visible: true } });
    });

    await waitFor(() => {
      expect(mocks.deviceAuth).toHaveBeenCalledTimes(2);
      expect(useAppLock.getState().appUnlocked).toBe(true);
    });
    expect(screen.queryByText("Notes is Locked")).toBeNull();
  });

  it("ignores visibility changes for other windows", async () => {
    await renderLockedGate();
    const emit = getVisibilityHandler();

    await act(async () => {
      emit({ payload: { window: { type: "composer" }, visible: false } });
    });

    expect(useAppLock.getState().appUnlocked).toBe(true);
    expect(mocks.deviceAuth).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("Notes is Locked")).toBeNull();
  });

  it("stays locked with a clear message when device authentication is unavailable", async () => {
    mocks.unlockAppInRust.mockResolvedValueOnce({
      ok: false,
      error: "aspc.auth_unavailable: device authentication is not available",
    } as never);
    render(
      <AppLockGate>
        <div>app content</div>
      </AppLockGate>,
    );

    await waitFor(() => {
      expect(screen.getByText("Notes is Locked")).toBeTruthy();
      expect(useAppLock.getState().unlockError).toContain(
        "aspc.auth_unavailable",
      );
    });
    expect(useAppLock.getState().appUnlocked).toBe(false);
    expect(screen.queryByText("app content")).toBeNull();
  });

  it("opens without prompting when the lock is off in Rust", async () => {
    mocks.rust.enabled = false;
    mocks.rust.unlocked = true;
    useAppLock.setState({ lockEnabled: false, appUnlocked: true });
    render(
      <AppLockGate>
        <div>app content</div>
      </AppLockGate>,
    );

    await waitFor(() => {
      expect(screen.getByText("app content")).toBeTruthy();
    });
    expect(mocks.deviceAuth).not.toHaveBeenCalled();
  });

  it("reloads data after unlocking from a relock", async () => {
    await renderLockedGate();
    const emit = getVisibilityHandler();

    await act(async () => {
      emit({ payload: { window: { type: "main" }, visible: false } });
    });
    expect(mocks.restartLiveQueries).not.toHaveBeenCalled();

    await act(async () => {
      emit({ payload: { window: { type: "main" }, visible: true } });
    });

    await waitFor(() => {
      expect(useAppLock.getState().appUnlocked).toBe(true);
      expect(mocks.restartLiveQueries).toHaveBeenCalledTimes(1);
      expect(mocks.invalidateQueries).toHaveBeenCalledTimes(1);
    });
  });
});
