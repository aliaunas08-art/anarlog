import { useEffect, useState } from "react";

import { commands as store2Commands } from "@anlg/plugin-store2";

// A "running" flag is set when the main window starts and cleared during a
// clean exit (see app-exit.ts / relaunch.ts). If it is still set on the next
// launch, the previous run ended unexpectedly (crash, force quit, power loss).
const SCOPE = "app-lifecycle";
const KEY = "running";

let detection: Promise<boolean> | null = null;
let dismissed = false;

export function detectUnexpectedQuit(): Promise<boolean> {
  detection ??= (async () => {
    try {
      const previous = await store2Commands.getBool(SCOPE, KEY);
      const wasRunning = previous.status === "ok" && previous.data === true;
      await store2Commands.setBool(SCOPE, KEY, true);
      await store2Commands.save();
      return wasRunning;
    } catch (error) {
      console.warn("[lifecycle] unexpected-quit detection failed", error);
      return false;
    }
  })();
  return detection;
}

export async function markCleanExit(): Promise<void> {
  try {
    await store2Commands.setBool(SCOPE, KEY, false);
  } catch (error) {
    console.warn("[lifecycle] failed to mark clean exit", error);
  }
}

export function dismissUnexpectedQuit() {
  dismissed = true;
}

/** `null` while unknown, then whether the last run quit unexpectedly. */
export function useUnexpectedQuit(): boolean | null {
  const [value, setValue] = useState<boolean | null>(null);
  useEffect(() => {
    let active = true;
    void detectUnexpectedQuit().then((quit) => {
      if (active) setValue(quit && !dismissed);
    });
    return () => {
      active = false;
    };
  }, []);
  return value;
}
