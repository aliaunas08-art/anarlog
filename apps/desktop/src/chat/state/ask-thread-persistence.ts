import { commands as store2Commands } from "@anlg/plugin-store2";

import { type ChatSelection, useChatContext } from "./chat-context";

// Ask threads (key → groupId/sessionId) survive restarts in store2, so a
// meeting's "This meeting" thread reopens where it was left.
const SCOPE = "chat";
const KEY = "ask-threads";
const MAX_THREADS = 200;
const SAVE_DELAY_MS = 500;

let started = false;

function parseThreads(json: string | null | undefined) {
  if (!json) return {};
  try {
    const parsed = JSON.parse(json) as Record<string, unknown>;
    const threads: Record<string, ChatSelection> = {};
    for (const [key, value] of Object.entries(parsed)) {
      if (
        typeof value === "object" &&
        value !== null &&
        typeof (value as ChatSelection).groupId === "string" &&
        typeof (value as ChatSelection).sessionId === "string"
      ) {
        const { groupId, sessionId } = value as ChatSelection;
        threads[key] = { groupId, sessionId };
      }
    }
    return threads;
  } catch {
    return {};
  }
}

/**
 * Only threads with a persisted group. A read-only previous chat is never
 * saved; the thread it is covering for is saved in its place.
 */
export function serializeAskThreads(
  threads: Record<string, ChatSelection>,
): string {
  const entries = Object.entries(threads)
    .map(
      ([key, selection]) =>
        [key, selection.legacy ? selection.displaced : selection] as const,
    )
    .filter(
      (entry): entry is readonly [string, ChatSelection] =>
        Boolean(entry[1]?.groupId) && !entry[1]?.legacy,
    )
    .slice(-MAX_THREADS)
    .map(([key, { groupId, sessionId }]) => [key, { groupId, sessionId }]);
  return JSON.stringify(Object.fromEntries(entries));
}

async function loadPersistedThreads() {
  try {
    const result = await store2Commands.getStr(SCOPE, KEY);
    if (result?.status !== "ok") return;
    const persisted = parseThreads(result.data);
    // Threads touched in this run win over the stored ones.
    useChatContext.setState((state) => ({
      askThreads: { ...persisted, ...state.askThreads },
    }));
  } catch (error) {
    console.warn("[ask-threads] failed to load", error);
  }
}

async function savePersistedThreads() {
  try {
    const json = serializeAskThreads(useChatContext.getState().askThreads);
    await store2Commands.setStr(SCOPE, KEY, json);
    await store2Commands.save();
  } catch (error) {
    console.warn("[ask-threads] failed to save", error);
  }
}

export function startAskThreadPersistence() {
  if (started || typeof useChatContext.subscribe !== "function") return;
  started = true;

  void loadPersistedThreads();

  let timer: ReturnType<typeof setTimeout> | undefined;
  useChatContext.subscribe((state, previous) => {
    if (state.askThreads === previous.askThreads) return;
    clearTimeout(timer);
    timer = setTimeout(() => {
      void savePersistedThreads();
    }, SAVE_DELAY_MS);
  });
}
