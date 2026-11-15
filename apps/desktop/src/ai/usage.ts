import type { LanguageModelMiddleware } from "ai";

import { commands as store2Commands } from "@anlg/plugin-store2";

// Local per-day AI usage counter (store2 key-value, no schema). Every model
// call made through createLanguageModel is recorded. Only the hosted developer
// key ("anarlog") can be limited; BYOK providers record but never block.

export const DEVELOPER_KEY_PROVIDER_ID = "anarlog";
const STORE_SCOPE = "ai-usage";

export type AiUsageDay = {
  date: string;
  calls: number;
  developerKeyCalls: number;
  inputTokens: number;
  outputTokens: number;
  byTask: Record<string, number>;
  byProvider: Record<string, number>;
  byAskScope: Record<string, number>;
  /** Ask redirects answered locally; they make no model call. */
  askRedirects: number;
};

export type AiUsageStorage = {
  load: (date: string) => Promise<string | null>;
  save: (date: string, json: string) => Promise<void>;
};

const store2Storage: AiUsageStorage = {
  load: async (date) => {
    const result = await store2Commands.getStr(STORE_SCOPE, `day:${date}`);
    return result?.status === "ok" ? (result.data ?? null) : null;
  },
  save: async (date, json) => {
    await store2Commands.setStr(STORE_SCOPE, `day:${date}`, json);
    await store2Commands.save();
  },
};

let storage: AiUsageStorage = store2Storage;
const days = new Map<string, AiUsageDay>();
const loads = new Map<string, Promise<AiUsageDay>>();
const loadedDates = new Set<string>();
let writeChain: Promise<void> = Promise.resolve();

export function todayKey(now: Date = new Date()): string {
  const y = now.getFullYear();
  const m = String(now.getMonth() + 1).padStart(2, "0");
  const d = String(now.getDate()).padStart(2, "0");
  return `${y}-${m}-${d}`;
}

function emptyDay(date: string): AiUsageDay {
  return {
    date,
    calls: 0,
    developerKeyCalls: 0,
    inputTokens: 0,
    outputTokens: 0,
    byTask: {},
    byProvider: {},
    byAskScope: {},
    askRedirects: 0,
  };
}

function parseDay(date: string, json: string | null): AiUsageDay {
  if (!json) return emptyDay(date);
  try {
    const parsed = JSON.parse(json) as Partial<AiUsageDay>;
    return { ...emptyDay(date), ...parsed, date };
  } catch {
    return emptyDay(date);
  }
}

/** Loads (once) and returns the persisted counters for a day. */
export function loadAiUsage(date: string = todayKey()): Promise<AiUsageDay> {
  const cached = days.get(date);
  if (cached && loadedDates.has(date)) return Promise.resolve(cached);
  let pending = loads.get(date);
  if (!pending) {
    pending = storage
      .load(date)
      .catch(() => null)
      .then((json) => {
        // Calls recorded while loading are merged on top of the stored day.
        const stored = parseDay(date, json);
        const live = days.get(date);
        const merged = live ? mergeDays(stored, live) : stored;
        days.set(date, merged);
        loadedDates.add(date);
        loads.delete(date);
        return merged;
      });
    loads.set(date, pending);
  }
  return pending;
}

function mergeCounts(a: Record<string, number>, b: Record<string, number>) {
  const out = { ...a };
  for (const [key, value] of Object.entries(b)) {
    out[key] = (out[key] ?? 0) + value;
  }
  return out;
}

function mergeDays(a: AiUsageDay, b: AiUsageDay): AiUsageDay {
  return {
    date: a.date,
    calls: a.calls + b.calls,
    developerKeyCalls: a.developerKeyCalls + b.developerKeyCalls,
    inputTokens: a.inputTokens + b.inputTokens,
    outputTokens: a.outputTokens + b.outputTokens,
    byTask: mergeCounts(a.byTask, b.byTask),
    byProvider: mergeCounts(a.byProvider, b.byProvider),
    byAskScope: mergeCounts(a.byAskScope, b.byAskScope),
    askRedirects: a.askRedirects + b.askRedirects,
  };
}

/** Synchronous snapshot of a day's counters (what has been loaded/recorded). */
export function getAiUsage(date: string = todayKey()): AiUsageDay {
  return structuredClone(days.get(date) ?? emptyDay(date));
}

function update(date: string, mutate: (day: AiUsageDay) => void) {
  const day = days.get(date) ?? emptyDay(date);
  mutate(day);
  days.set(date, day);
  // The stored day is merged in before the first write lands.
  const loaded = loadAiUsage(date);
  writeChain = writeChain
    .then(() => loaded)
    .then(() => storage.save(date, JSON.stringify(days.get(date) ?? day)))
    .catch((error) => {
      console.warn("[ai-usage] failed to persist usage", error);
    });
}

export type AiCallRecord = {
  task?: string | null;
  providerId: string;
  askScope?: string | null;
  inputTokens?: number | null;
  outputTokens?: number | null;
};

export function recordAiCall(record: AiCallRecord, now: Date = new Date()) {
  update(todayKey(now), (day) => {
    day.calls += 1;
    if (record.providerId === DEVELOPER_KEY_PROVIDER_ID) {
      day.developerKeyCalls += 1;
    }
    day.inputTokens += record.inputTokens ?? 0;
    day.outputTokens += record.outputTokens ?? 0;
    const task = record.task ?? "unknown";
    day.byTask[task] = (day.byTask[task] ?? 0) + 1;
    day.byProvider[record.providerId] =
      (day.byProvider[record.providerId] ?? 0) + 1;
    if (record.askScope) {
      day.byAskScope[record.askScope] =
        (day.byAskScope[record.askScope] ?? 0) + 1;
    }
  });
}

export function recordAskRedirect(now: Date = new Date()) {
  update(todayKey(now), (day) => {
    day.askRedirects += 1;
  });
}

/**
 * Whether Ask must stop calling the model. Only the developer key is limited;
 * `limit` null/undefined/<= 0 means unlimited.
 */
export async function isAiUsageLimitReached({
  providerId,
  limit,
}: {
  providerId: string | null | undefined;
  limit: number | null | undefined;
}): Promise<boolean> {
  if (providerId !== DEVELOPER_KEY_PROVIDER_ID) return false;
  if (typeof limit !== "number" || !Number.isFinite(limit) || limit <= 0) {
    return false;
  }
  const day = await loadAiUsage();
  return day.developerKeyCalls >= limit;
}

// Ask passes its scope to the middleware through provider options.
export const USAGE_PROVIDER_OPTIONS_KEY = "anarlogUsage";

export function usageProviderOptions(askScope: string) {
  return { [USAGE_PROVIDER_OPTIONS_KEY]: { askScope } };
}

function readAskScope(providerOptions: unknown): string | null {
  if (typeof providerOptions !== "object" || providerOptions === null) {
    return null;
  }
  const entry = (providerOptions as Record<string, unknown>)[
    USAGE_PROVIDER_OPTIONS_KEY
  ];
  if (typeof entry !== "object" || entry === null) return null;
  const scope = (entry as Record<string, unknown>).askScope;
  return typeof scope === "string" ? scope : null;
}

// transformParams hands the same (transformed) params object to
// wrapGenerate/wrapStream, so the scope is carried by object identity.
const askScopeByParams = new WeakMap<object, string>();

function stripUsageOptions<T extends { providerOptions?: unknown }>(
  params: T,
): T {
  const options = params.providerOptions as Record<string, unknown> | undefined;
  if (!options || !(USAGE_PROVIDER_OPTIONS_KEY in options)) return params;
  const askScope = readAskScope(options);
  const { [USAGE_PROVIDER_OPTIONS_KEY]: _ignored, ...rest } = options;
  const stripped = { ...params, providerOptions: rest };
  if (askScope) askScopeByParams.set(stripped, askScope);
  return stripped;
}

type UsageLike = {
  inputTokens?: number | { total?: number | undefined } | undefined;
  outputTokens?: number | { total?: number | undefined } | undefined;
};

function tokenTotal(value: UsageLike["inputTokens"]): number | null {
  if (typeof value === "number") return value;
  if (value && typeof value.total === "number") return value.total;
  return null;
}

/** Records every generate/stream call made through the wrapped model. */
export function createUsageTrackingMiddleware({
  task,
  providerId,
}: {
  task?: string | null;
  providerId: string;
}): LanguageModelMiddleware {
  return {
    specificationVersion: "v3",
    transformParams: async ({ params }) => stripUsageOptions(params),
    wrapGenerate: async ({ doGenerate, params }) => {
      const askScope = askScopeByParams.get(params) ?? null;
      const result = await doGenerate();
      const usage = (result as { usage?: UsageLike }).usage;
      recordAiCall({
        task,
        providerId,
        askScope,
        inputTokens: tokenTotal(usage?.inputTokens),
        outputTokens: tokenTotal(usage?.outputTokens),
      });
      return result;
    },
    wrapStream: async ({ doStream, params }) => {
      const askScope = askScopeByParams.get(params) ?? null;
      const result = await doStream();
      let usage: UsageLike | undefined;
      let recorded = false;
      const record = () => {
        if (recorded) return;
        recorded = true;
        recordAiCall({
          task,
          providerId,
          askScope,
          inputTokens: tokenTotal(usage?.inputTokens),
          outputTokens: tokenTotal(usage?.outputTokens),
        });
      };
      const stream = result.stream.pipeThrough(
        new TransformStream({
          transform(chunk, controller) {
            if (
              typeof chunk === "object" &&
              chunk !== null &&
              (chunk as { type?: string }).type === "finish"
            ) {
              usage = (chunk as { usage?: UsageLike }).usage;
            }
            controller.enqueue(chunk);
          },
          flush() {
            record();
          },
          cancel() {
            record();
          },
        } as Transformer<unknown, unknown> & { cancel?: () => void }),
      ) as typeof result.stream;
      return { ...result, stream };
    },
  };
}

/** Test seam: swap persistence and clear in-memory counters. */
export function resetAiUsageForTests(nextStorage?: AiUsageStorage) {
  days.clear();
  loads.clear();
  loadedDates.clear();
  writeChain = Promise.resolve();
  storage = nextStorage ?? {
    load: async () => null,
    save: async () => undefined,
  };
}
