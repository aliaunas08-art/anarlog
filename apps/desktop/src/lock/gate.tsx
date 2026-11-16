import { useLingui } from "@lingui/react/macro";
import { useQueryClient } from "@tanstack/react-query";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { type ReactNode, useEffect, useRef, useState } from "react";

import {
  events as windowsEvents,
  getCurrentWebviewWindowLabel,
} from "@anlg/plugin-windows";

import { LockScreen, useDeviceAuthHint } from "./screen";
import { startRustLockSync, useAppLock } from "./store";

import { restartLiveQueries } from "~/db";
import { useSettingsReady } from "~/settings/queries";
import { APP_NAME } from "~/shared/app-name";
import { BrandLoadingView } from "~/shared/brand-loading-view";

// The lock state lives in Rust (ASPC A4.2). This gate only mirrors it: it
// shows the lock screen while Rust says locked, asks Rust to unlock (Rust
// runs Touch ID / the password prompt itself), and asks Rust to lock when
// the main window is hidden. Rust also relocks on its own (hide, sleep).
export function AppLockGate({ children }: { children: ReactNode }) {
  const { t } = useLingui();
  const queryClient = useQueryClient();
  const settingsReady = useSettingsReady();
  const lockEnabled = useAppLock((state) => state.lockEnabled);
  const appUnlocked = useAppLock((state) => state.appUnlocked);
  const authenticating = useAppLock((state) => state.authenticating);
  const unlockError = useAppLock((state) => state.unlockError);
  const unlockApp = useAppLock((state) => state.unlockApp);
  const lockApp = useAppLock((state) => state.lockApp);
  const hint = useDeviceAuthHint();
  const promptedRef = useRef(false);
  const visibilityEpochRef = useRef(0);
  const hiddenAtMountRef = useRef(false);
  const relockedRef = useRef(false);
  const [sessionStarted, setSessionStarted] = useState(false);

  useEffect(() => {
    void startRustLockSync();
  }, []);

  const shouldLock = lockEnabled === true;
  const locked = shouldLock && !appUnlocked;

  useEffect(() => {
    if (lockEnabled === null) return;
    if (locked) {
      if (sessionStarted) relockedRef.current = true;
      return;
    }
    setSessionStarted(true);
    if (relockedRef.current) {
      // Queries that refreshed while the webview SQL was locked failed;
      // load everything again now that Rust allows it.
      relockedRef.current = false;
      restartLiveQueries();
      void queryClient.invalidateQueries();
    }
  }, [lockEnabled, locked, sessionStarted, queryClient]);

  useEffect(() => {
    if (!locked || authenticating || promptedRef.current) {
      return;
    }
    promptedRef.current = true;
    if (getCurrentWebviewWindowLabel() !== "main") {
      void unlockApp();
      return;
    }
    // A main window rebuilt in the background must not prompt until the
    // user opens it; the visibility listener prompts then.
    promptIfMainVisible(visibilityEpochRef, hiddenAtMountRef);
  }, [unlockApp, locked, authenticating]);

  useEffect(() => {
    if (!locked) {
      promptedRef.current = false;
    }
  }, [locked]);

  useEffect(() => {
    if (!shouldLock) return;

    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void windowsEvents.visibilityEvent
      .listen(({ payload }) => {
        if (
          payload.window.type !== "main" ||
          getCurrentWebviewWindowLabel() !== "main"
        ) {
          return;
        }
        visibilityEpochRef.current += 1;
        hiddenAtMountRef.current = false;

        if (!payload.visible) {
          // Closing hides the main window. Lock now, but do not prompt until
          // the user opens it again.
          promptedRef.current = true;
          void lockApp();
          return;
        }

        if (useAppLock.getState().appUnlocked) return;
        if (useAppLock.getState().authenticating) {
          // A close-invalidated prompt is still running. Let the auto-prompt
          // effect start a new one once that auth settles.
          promptedRef.current = false;
          return;
        }
        promptedRef.current = true;
        void useAppLock.getState().unlockApp();
      })
      .then((fn) => {
        if (cancelled) {
          fn();
          return;
        }
        unlisten = fn;
        // A show event emitted before the listener was registered is lost.
        if (hiddenAtMountRef.current) {
          promptIfMainVisible(visibilityEpochRef, hiddenAtMountRef);
        }
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [lockApp, shouldLock]);

  if (!settingsReady || lockEnabled === null) {
    return <BrandLoadingView />;
  }

  return (
    <div className="relative h-full min-h-0 w-full">
      {sessionStarted ? (
        <div
          className="h-full min-h-0 w-full"
          {...(locked ? { inert: true } : {})}
        >
          {children}
        </div>
      ) : null}
      {locked ? (
        <div className="absolute inset-0">
          <LockScreen
            title={t`${APP_NAME} is Locked`}
            description={
              unlockError?.includes("aspc.auth_unavailable")
                ? t`Device authentication is not available on this computer, so ${APP_NAME} stays locked. Set a login password or Touch ID in System Settings, then try again.`
                : hint
            }
            action={t`View ${APP_NAME}`}
            authenticating={authenticating}
            onUnlock={() => {
              promptedRef.current = true;
              void unlockApp();
            }}
          />
        </div>
      ) : null}
    </div>
  );
}

function promptIfMainVisible(
  visibilityEpochRef: { current: number },
  hiddenAtMountRef: { current: boolean },
) {
  const epoch = visibilityEpochRef.current;
  void getCurrentWebviewWindow()
    .isVisible()
    .catch(() => true)
    .then((visible) => {
      if (epoch !== visibilityEpochRef.current) return;
      if (!visible) {
        hiddenAtMountRef.current = true;
        return;
      }
      hiddenAtMountRef.current = false;
      const state = useAppLock.getState();
      if (state.appUnlocked || state.authenticating) return;
      void state.unlockApp();
    });
}
