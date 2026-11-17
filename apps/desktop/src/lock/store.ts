import { create } from "zustand";

import {
  authenticateDevice,
  getRustLockState,
  isDeviceAuthAvailable,
  listenRustLockState,
  lockAppInRust,
  type RustLockState,
  setAppLockEnabledInRust,
  unlockAppInRust,
} from "./auth";

import { flushDatabaseWritesWithin } from "~/db/write-queue";

const LOCK_FLUSH_TIMEOUT_MS = 3_000;

type AppLockState = {
  available: boolean | null;
  authenticating: boolean;
  /** Mirrors Rust. `null` until the first read. */
  lockEnabled: boolean | null;
  /** Mirrors Rust: true when the lock is off or has been passed. */
  appUnlocked: boolean;
  unlockError: string | null;
  revealedNoteIds: Record<string, true>;
  refreshAvailability: () => Promise<boolean>;
  authenticate: (reason: string) => Promise<boolean>;
  syncFromRust: () => Promise<void>;
  applyRustState: (state: RustLockState) => void;
  unlockApp: (reason?: string) => Promise<boolean>;
  lockApp: () => Promise<void>;
  setLockEnabled: (enabled: boolean) => Promise<boolean>;
  revealNote: (sessionId: string, reason: string) => Promise<boolean>;
  markNoteRevealed: (sessionId: string) => void;
  concealNote: (sessionId: string) => void;
  isNoteRevealed: (sessionId: string) => boolean;
};

export const useAppLock = create<AppLockState>((set, get) => {
  let lockEpoch = 0;

  return {
    available: null,
    authenticating: false,
    lockEnabled: null,
    appUnlocked: false,
    unlockError: null,
    revealedNoteIds: {},
    refreshAvailability: async () => {
      try {
        const available = await isDeviceAuthAvailable();
        set({ available });
        return available;
      } catch {
        set({ available: false });
        return false;
      }
    },
    authenticate: async (reason) => {
      if (get().authenticating) return false;
      set({ authenticating: true });
      try {
        return await authenticateDevice(reason);
      } finally {
        set({ authenticating: false });
      }
    },
    syncFromRust: async () => {
      try {
        get().applyRustState(await getRustLockState());
      } catch (error) {
        // Without the Rust state the gate keeps showing the loading view;
        // it never opens on its own.
        set({ unlockError: String(error) });
      }
    },
    applyRustState: (state) => {
      const wasUnlocked = get().appUnlocked;
      if (!state.unlocked && wasUnlocked) {
        lockEpoch += 1;
        set({ revealedNoteIds: {} });
      }
      set({ lockEnabled: state.enabled, appUnlocked: state.unlocked });
      if (state.unlocked) {
        set({ unlockError: null });
      }
    },
    unlockApp: async () => {
      if (get().authenticating) return false;
      set({ authenticating: true, unlockError: null });
      try {
        const result = await unlockAppInRust();
        if (!result.ok) {
          set({ unlockError: result.error });
          return false;
        }
        get().applyRustState(result.state);
        return result.state.unlocked;
      } finally {
        set({ authenticating: false });
      }
    },
    lockApp: async () => {
      if (!get().lockEnabled) return;
      lockEpoch += 1;
      set({ appUnlocked: false, revealedNoteIds: {} });
      // Let pending edits reach SQLite before Rust denies webview writes.
      await flushDatabaseWritesWithin(LOCK_FLUSH_TIMEOUT_MS).catch(() => {});
      const state = await lockAppInRust();
      if (state) get().applyRustState(state);
    },
    setLockEnabled: async (enabled) => {
      if (get().authenticating) return false;
      set({ authenticating: true, unlockError: null });
      try {
        const result = await setAppLockEnabledInRust(enabled);
        if (!result.ok) {
          set({ unlockError: result.error });
          return false;
        }
        get().applyRustState(result.state);
        return result.state.enabled === enabled;
      } finally {
        set({ authenticating: false });
      }
    },
    revealNote: async (sessionId, reason) => {
      if (get().revealedNoteIds[sessionId]) return true;
      const epoch = lockEpoch;
      const ok = await get().authenticate(reason);
      if (ok && epoch === lockEpoch) {
        get().markNoteRevealed(sessionId);
        return true;
      }
      return false;
    },
    markNoteRevealed: (sessionId) => {
      set((state) => ({
        revealedNoteIds: { ...state.revealedNoteIds, [sessionId]: true },
      }));
    },
    concealNote: (sessionId) => {
      set((state) => {
        if (!state.revealedNoteIds[sessionId]) return state;
        const revealedNoteIds = { ...state.revealedNoteIds };
        delete revealedNoteIds[sessionId];
        return { revealedNoteIds };
      });
    },
    isNoteRevealed: (sessionId) => Boolean(get().revealedNoteIds[sessionId]),
  };
});

let rustSync: Promise<void> | null = null;

/** Reads the Rust lock state once and follows its change events. */
export function startRustLockSync(): Promise<void> {
  if (!rustSync) {
    rustSync = (async () => {
      await listenRustLockState((state) =>
        useAppLock.getState().applyRustState(state),
      ).catch(() => {});
      await useAppLock.getState().syncFromRust();
    })();
  }
  return rustSync;
}

/** Resolves once Rust reports the app unlocked (immediately if the lock is off). */
export function waitForAppUnlock(): Promise<void> {
  void startRustLockSync();
  return new Promise((resolve) => {
    const check = (state: AppLockState) =>
      state.lockEnabled !== null && state.appUnlocked;
    if (check(useAppLock.getState())) {
      resolve();
      return;
    }
    const unsubscribe = useAppLock.subscribe((state) => {
      if (check(state)) {
        unsubscribe();
        resolve();
      }
    });
  });
}
