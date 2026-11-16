import { commands, events } from "@anlg/plugin-local-auth";

export const DEVICE_AUTH_REASON = {
  openApp: "open",
  lockNote: "lock this note",
  unlockNote: "unlock this note",
  changeLockSettings: "change lock settings",
} as const;

export type RustLockState = { enabled: boolean; unlocked: boolean };

export async function isDeviceAuthAvailable(): Promise<boolean> {
  const result = await commands.available();
  return result.status === "ok" && result.data;
}

/** One-off authentication (note locks). Never unlocks the app. */
export async function authenticateDevice(reason: string): Promise<boolean> {
  const result = await commands.authenticate(reason);
  return result.status === "ok" && result.data === true;
}

// The app lock lives in Rust (ASPC A4.2). These only ask Rust; the webview
// can lock but never unlock by itself.

export async function getRustLockState(): Promise<RustLockState> {
  const result = await commands.appLockState();
  if (result.status === "error") throw new Error(result.error);
  return result.data;
}

export async function unlockAppInRust(): Promise<
  { ok: true; state: RustLockState } | { ok: false; error: string }
> {
  const result = await commands.unlockApp();
  return result.status === "ok"
    ? { ok: true, state: result.data }
    : { ok: false, error: result.error };
}

export async function lockAppInRust(): Promise<RustLockState | null> {
  const result = await commands.lockApp();
  return result.status === "ok" ? result.data : null;
}

export async function setAppLockEnabledInRust(
  enabled: boolean,
): Promise<{ ok: true; state: RustLockState } | { ok: false; error: string }> {
  const result = await commands.setAppLockEnabled(enabled);
  return result.status === "ok"
    ? { ok: true, state: result.data }
    : { ok: false, error: result.error };
}

export function listenRustLockState(
  onChange: (state: RustLockState) => void,
): Promise<() => void> {
  return events.appLockChanged.listen(({ payload }) => onChange(payload));
}
